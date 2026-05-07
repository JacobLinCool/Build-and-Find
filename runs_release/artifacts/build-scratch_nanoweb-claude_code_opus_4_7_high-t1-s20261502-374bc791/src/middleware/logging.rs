//! Structured request logging middleware.

use std::time::Instant;

use crate::error::Result;
use crate::http::{Request, Response};
use crate::log;

use super::{BoxFuture, Middleware, Next};

pub struct LoggingMiddleware;

impl LoggingMiddleware {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LoggingMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl Middleware for LoggingMiddleware {
    fn handle<'a>(
        &'a self,
        req: &'a mut Request,
        next: Next<'a>,
    ) -> BoxFuture<'a, Result<Response>> {
        Box::pin(async move {
            let start = Instant::now();
            let method = req.method;
            let path = req.path.clone();
            let request_id = req.request_id.clone();
            let remote_ip = req.remote_ip.clone();
            let result = next.run(req).await;
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            match &result {
                Ok(resp) => {
                    log::info(&format!(
                        "request id={} method={} path={} status={} ip={} duration_ms={:.2}",
                        request_id, method, path, resp.status.0, remote_ip, elapsed_ms
                    ));
                }
                Err(e) => {
                    log::error(&format!(
                        "request id={} method={} path={} error=\"{}\" ip={} duration_ms={:.2}",
                        request_id, method, path, e, remote_ip, elapsed_ms
                    ));
                }
            }
            result
        })
    }
}
