pub mod basic_auth;
pub mod cors;
pub mod csrf;
pub mod gzip;
pub mod logging;
pub mod rate_limiting;

use crate::error::NanowebError;
use crate::http::types::{Request, Response};
use async_trait::async_trait;
use std::sync::Arc;

/// Next represents the rest of the middleware chain.
pub struct Next<'a> {
    pub(crate) middlewares: &'a [Arc<dyn Middleware>],
    pub(crate) handler: &'a (dyn Fn(
        Request,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Response, NanowebError>> + Send>,
    > + Send
             + Sync),
}

impl<'a> Next<'a> {
    pub async fn run(self, req: Request) -> Result<Response, NanowebError> {
        if let Some((first, rest)) = self.middlewares.split_first() {
            let next = Next {
                middlewares: rest,
                handler: self.handler,
            };
            first.handle(req, next).await
        } else {
            (self.handler)(req).await
        }
    }
}

#[async_trait]
pub trait Middleware: Send + Sync {
    async fn handle(&self, req: Request, next: Next<'_>) -> Result<Response, NanowebError>;
}

pub struct MiddlewareStack {
    middlewares: Vec<Arc<dyn Middleware>>,
}

impl MiddlewareStack {
    pub fn new() -> Self {
        MiddlewareStack {
            middlewares: Vec::new(),
        }
    }

    pub fn add(&mut self, m: impl Middleware + 'static) {
        self.middlewares.push(Arc::new(m));
    }

    pub async fn run<F, Fut>(&self, req: Request, handler: F) -> Result<Response, NanowebError>
    where
        F: Fn(Request) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<Response, NanowebError>> + Send + 'static,
    {
        let handler_arc: Arc<
            dyn Fn(
                    Request,
                ) -> std::pin::Pin<
                    Box<dyn std::future::Future<Output = Result<Response, NanowebError>> + Send>,
                > + Send
                + Sync,
        > = Arc::new(move |req| Box::pin(handler(req)));

        let next = Next {
            middlewares: &self.middlewares,
            handler: &*handler_arc,
        };
        next.run(req).await
    }
}

impl Default for MiddlewareStack {
    fn default() -> Self {
        MiddlewareStack::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::{Method, Request, Response};

    struct AddHeader(&'static str, &'static str);

    #[async_trait]
    impl Middleware for AddHeader {
        async fn handle(&self, req: Request, next: Next<'_>) -> Result<Response, NanowebError> {
            let mut resp = next.run(req).await?;
            resp.headers.insert(self.0.to_string(), self.1.to_string());
            Ok(resp)
        }
    }

    #[tokio::test]
    async fn test_middleware_chain() {
        let mut stack = MiddlewareStack::new();
        stack.add(AddHeader("x-first", "1"));
        stack.add(AddHeader("x-second", "2"));

        let handler = |_req: Request| async { Ok(Response::ok().with_text("hello")) };
        let req = Request::new(Method::GET, "/".to_string());
        let resp = stack.run(req, handler).await.unwrap();
        assert_eq!(resp.headers.get("x-first"), Some(&"1".to_string()));
        assert_eq!(resp.headers.get("x-second"), Some(&"2".to_string()));
    }
}
