pub mod auth;
pub mod compression;
pub mod cors;
pub mod csrf;
pub mod logging;
pub mod rate_limit;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::NanowebError;
use crate::http::types::{Request, Response};

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Next is a function that calls the next middleware or handler
pub type NextFn = Arc<dyn Fn(Request) -> BoxFuture<Result<Response, NanowebError>> + Send + Sync>;

pub struct Next(pub NextFn);

impl Next {
    pub async fn run(self, req: Request) -> Result<Response, NanowebError> {
        (self.0)(req).await
    }
}

/// Object-safe async middleware trait
pub trait Middleware: Send + Sync {
    fn handle<'a>(
        &'a self,
        req: Request,
        next: Next,
    ) -> BoxFuture<Result<Response, NanowebError>>;
}

/// Stack of middleware + final handler
pub struct MiddlewareStack {
    middlewares: Vec<Arc<dyn Middleware + Send + Sync>>,
}

impl MiddlewareStack {
    pub fn new() -> Self {
        MiddlewareStack {
            middlewares: Vec::new(),
        }
    }

    pub fn add<M: Middleware + Send + Sync + 'static>(&mut self, middleware: M) {
        self.middlewares.push(Arc::new(middleware));
    }

    pub fn run(
        &self,
        req: Request,
        handler: Arc<dyn Fn(Request) -> BoxFuture<Result<Response, NanowebError>> + Send + Sync>,
    ) -> BoxFuture<Result<Response, NanowebError>> {
        let middlewares = self.middlewares.clone();
        build_chain(middlewares, 0, req, handler)
    }
}

fn build_chain(
    middlewares: Vec<Arc<dyn Middleware + Send + Sync>>,
    idx: usize,
    req: Request,
    handler: Arc<dyn Fn(Request) -> BoxFuture<Result<Response, NanowebError>> + Send + Sync>,
) -> BoxFuture<Result<Response, NanowebError>> {
    Box::pin(async move {
        if idx >= middlewares.len() {
            handler(req).await
        } else {
            let middleware = middlewares[idx].clone();
            let middlewares_clone = middlewares.clone();
            let handler_clone = handler.clone();
            let next = Next(Arc::new(move |next_req| {
                build_chain(
                    middlewares_clone.clone(),
                    idx + 1,
                    next_req,
                    handler_clone.clone(),
                )
            }));
            middleware.handle(req, next).await
        }
    })
}

impl Default for MiddlewareStack {
    fn default() -> Self {
        Self::new()
    }
}
