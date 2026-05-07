pub mod basic_auth;
pub mod cors;
pub mod csrf;
pub mod gzip;
pub mod logging;
pub mod rate_limit;

use crate::http::{Request, Response};
use async_trait::async_trait;

#[async_trait]
pub trait Middleware: Send + Sync {
    async fn before(&self, _req: &mut Request) -> Option<Response> {
        None
    }

    async fn after(&self, _req: &Request, _resp: &mut Response) {}
}

pub type MiddlewareChain = Vec<Box<dyn Middleware>>;

pub async fn run_before(chain: &MiddlewareChain, req: &mut Request) -> Option<Response> {
    for mw in chain {
        if let Some(resp) = mw.before(req).await {
            return Some(resp);
        }
    }
    None
}

pub async fn run_after(chain: &MiddlewareChain, req: &Request, resp: &mut Response) {
    // Run in reverse order for after hooks
    for mw in chain.iter().rev() {
        mw.after(req, resp).await;
    }
}
