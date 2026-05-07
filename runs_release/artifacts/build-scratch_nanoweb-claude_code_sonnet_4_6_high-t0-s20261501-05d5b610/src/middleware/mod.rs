pub mod basic_auth;
pub mod compression;
pub mod cors;
pub mod csrf;
pub mod logging;
pub mod rate_limit;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::NanowebError;
use crate::http::{Request, Response};
use crate::router::HandlerFn;

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Core middleware trait. Implementations run logic before and/or after the inner handler.
/// The order is deterministic: middleware registered first wraps outermost.
#[async_trait::async_trait]
pub trait Middleware: Send + Sync {
    async fn handle(&self, req: Request, next: Next<'_>) -> Result<Response, NanowebError>;
}

/// Carries the remaining middleware chain and the final handler.
pub struct Next<'a> {
    pub handler: &'a HandlerFn,
    pub middleware: &'a [Arc<dyn Middleware>],
}

impl<'a> Next<'a> {
    pub async fn run(self, req: Request) -> Result<Response, NanowebError> {
        match self.middleware.split_first() {
            Some((first, rest)) => {
                first
                    .handle(
                        req,
                        Next {
                            handler: self.handler,
                            middleware: rest,
                        },
                    )
                    .await
            }
            None => (self.handler)(req).await,
        }
    }
}

/// Owned middleware chain that can be called without lifetime params.
pub struct MiddlewareChain {
    middleware: Vec<Arc<dyn Middleware>>,
    handler: HandlerFn,
}

impl MiddlewareChain {
    pub fn new(middleware: Vec<Arc<dyn Middleware>>, handler: HandlerFn) -> Self {
        Self {
            middleware,
            handler,
        }
    }

    pub async fn run(self, req: Request) -> Result<Response, NanowebError> {
        let next = Next {
            handler: &self.handler,
            middleware: &self.middleware,
        };
        next.run(req).await
    }
}

pub use basic_auth::BasicAuthMiddleware;
pub use compression::CompressionMiddleware;
pub use cors::CorsMiddleware;
pub use csrf::CsrfMiddleware;
pub use logging::LoggingMiddleware;
pub use rate_limit::RateLimitMiddleware;
