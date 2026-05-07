pub mod basic_auth;
pub mod cors;
pub mod csrf;
pub mod gzip;
pub mod logging;
pub mod rate_limit;

use crate::error::NanowebError;
use crate::http::types::{Request, Response};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

pub trait Handler: Send + Sync + 'static {
    fn call(&self, req: Request) -> BoxFuture<Result<Response, NanowebError>>;
}

pub struct FnHandler<F>(F);

impl<F> FnHandler<F> {
    pub fn new(f: F) -> Self {
        FnHandler(f)
    }
}

impl<F, Fut> Handler for FnHandler<F>
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response, NanowebError>> + Send + 'static,
{
    fn call(&self, req: Request) -> BoxFuture<Result<Response, NanowebError>> {
        Box::pin((self.0)(req))
    }
}

pub trait Middleware: Send + Sync + 'static {
    fn call(&self, req: Request, next: Next) -> BoxFuture<Result<Response, NanowebError>>;
}

#[derive(Clone)]
pub struct Next {
    pub handler: Arc<dyn Handler>,
    pub stack: Arc<Vec<Arc<dyn Middleware>>>,
    pub pos: usize,
}

impl Next {
    pub fn new(handler: Arc<dyn Handler>, stack: Vec<Arc<dyn Middleware>>) -> Self {
        Next {
            handler,
            stack: Arc::new(stack),
            pos: 0,
        }
    }

    pub fn run(self, req: Request) -> BoxFuture<Result<Response, NanowebError>> {
        Box::pin(async move {
            if self.pos < self.stack.len() {
                let m = self.stack[self.pos].clone();
                let next = Next {
                    pos: self.pos + 1,
                    ..self
                };
                m.call(req, next).await
            } else {
                self.handler.call(req).await
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DoubleMiddleware;

    impl Middleware for DoubleMiddleware {
        fn call(&self, req: Request, next: Next) -> BoxFuture<Result<Response, NanowebError>> {
            Box::pin(async move {
                let mut resp = next.run(req).await?;
                resp.body.extend_from_slice(b"!");
                Ok(resp)
            })
        }
    }

    #[tokio::test]
    async fn test_middleware_chain() {
        use crate::http::types::{HttpVersion, Method};
        use std::collections::HashMap;

        let handler = Arc::new(FnHandler::new(|_req: Request| async {
            Ok(Response::text("hello"))
        }));

        let req = Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            query: None,
            version: HttpVersion::Http11,
            headers: vec![],
            body: vec![],
            params: HashMap::new(),
            remote_addr: "127.0.0.1".into(),
            request_id: "test".into(),
        };

        let middleware: Vec<Arc<dyn Middleware>> = vec![Arc::new(DoubleMiddleware)];
        let next = Next::new(handler, middleware);
        let resp = next.run(req).await.unwrap();
        assert!(resp.body.ends_with(b"!"));
    }
}
