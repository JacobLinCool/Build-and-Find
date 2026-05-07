//! Middleware trait and standard middleware library.

pub mod basic_auth;
pub mod compression;
pub mod cors;
pub mod csrf;
pub mod logging;
pub mod rate_limit;

use crate::http::{Request, Response};
use crate::NanowebError;
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait Middleware: Send + Sync + 'static {
    /// Run before the handler. Return Ok(Some(resp)) to short-circuit.
    async fn before(&self, _req: &mut Request) -> Result<Option<Response>, NanowebError> {
        Ok(None)
    }

    /// Run after the handler.
    async fn after(&self, _req: &Request, resp: Response) -> Result<Response, NanowebError> {
        Ok(resp)
    }
}

pub type ArcMiddleware = Arc<dyn Middleware>;
