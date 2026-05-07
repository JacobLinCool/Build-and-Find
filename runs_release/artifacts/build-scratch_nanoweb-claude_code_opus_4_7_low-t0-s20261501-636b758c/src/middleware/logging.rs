use super::Middleware;
use crate::http::{Request, Response};
use crate::NanowebError;
use async_trait::async_trait;

pub struct RequestLogger;

#[async_trait]
impl Middleware for RequestLogger {
    async fn before(&self, req: &mut Request) -> Result<Option<Response>, NanowebError> {
        tracing::info!(
            request_id = %req.request_id,
            method = %req.method,
            path = %req.path,
            "request received"
        );
        Ok(None)
    }
    async fn after(&self, req: &Request, resp: Response) -> Result<Response, NanowebError> {
        tracing::info!(
            request_id = %req.request_id,
            method = %req.method,
            path = %req.path,
            status = resp.status.0,
            "response sent"
        );
        Ok(resp)
    }
}
