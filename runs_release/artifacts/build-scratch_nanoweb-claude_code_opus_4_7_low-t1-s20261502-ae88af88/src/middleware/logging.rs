use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;
use std::time::Instant;

pub struct Logging;

#[async_trait]
impl Middleware for Logging {
    async fn before(&self, req: &mut Request) -> Option<Response> {
        req.extensions
            .set("__start", Instant::now().elapsed().as_nanos().to_string());
        tracing::info!(
            method = %req.method,
            path = %req.path,
            request_id = %req.request_id,
            "request_start"
        );
        None
    }

    async fn after(&self, req: &Request, res: &mut Response) {
        tracing::info!(
            method = %req.method,
            path = %req.path,
            status = res.status.0,
            request_id = %req.request_id,
            "request_end"
        );
    }
}
