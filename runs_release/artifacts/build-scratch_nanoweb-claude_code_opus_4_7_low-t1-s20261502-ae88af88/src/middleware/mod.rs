use crate::http::{Request, Response};
use async_trait::async_trait;
use std::sync::Arc;

pub mod basic_auth;
pub mod compression;
pub mod cors;
pub mod csrf;
pub mod logging;
pub mod rate_limit;

pub use basic_auth::BasicAuth;
pub use compression::Gzip;
pub use cors::Cors;
pub use csrf::Csrf;
pub use logging::Logging;
pub use rate_limit::RateLimit;

/// A middleware can short-circuit by returning Some(Response) before the handler,
/// and observe/mutate the response in `after`.
#[async_trait]
pub trait Middleware: Send + Sync + 'static {
    async fn before(&self, _req: &mut Request) -> Option<Response> {
        None
    }
    async fn after(&self, _req: &Request, _res: &mut Response) {}
}

#[derive(Clone, Default)]
pub struct Stack {
    pub items: Vec<Arc<dyn Middleware>>,
}

impl Stack {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn push(&mut self, m: Arc<dyn Middleware>) {
        self.items.push(m);
    }
    pub fn append(mut self, m: Arc<dyn Middleware>) -> Self {
        self.items.push(m);
        self
    }
}
