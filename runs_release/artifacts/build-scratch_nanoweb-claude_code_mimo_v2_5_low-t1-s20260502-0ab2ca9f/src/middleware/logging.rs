use crate::http::{Request, Response};
use crate::middleware::Middleware;

pub struct LoggingMiddleware;

impl Middleware for LoggingMiddleware {
    fn before(&self, req: &mut Request) -> Option<Response> {
        tracing::info!(
            method = %req.method,
            path = %req.path,
            request_id = %req.request_id,
            "request received"
        );
        None
    }

    fn after(&self, req: &Request, res: &mut Response) {
        tracing::info!(
            method = %req.method,
            path = %req.path,
            status = %res.status,
            request_id = %req.request_id,
            "request completed"
        );
    }
}

impl LoggingMiddleware {
    pub fn new() -> Self {
        LoggingMiddleware
    }
}
