use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;
use std::time::Instant;

pub struct RequestLogger;

impl RequestLogger {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RequestLogger {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for RequestLogger {
    fn name(&self) -> &'static str {
        "request_logger"
    }

    async fn before(&self, req: &mut Request) -> Option<Response> {
        req.extensions.insert(
            "logger.start",
            &Instant::now().elapsed().as_nanos().to_string(),
        );
        // Use a sentinel timestamp via system instant; we re-derive on after.
        tracing::info!(
            method = %req.method,
            path = %req.path,
            request_id = %req.request_id,
            "request received"
        );
        None
    }

    async fn after(&self, req: &Request, resp: &mut Response) {
        tracing::info!(
            method = %req.method,
            path = %req.path,
            status = resp.status.as_u16(),
            request_id = %req.request_id,
            "request completed"
        );
    }
}
