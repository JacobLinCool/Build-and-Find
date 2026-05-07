use crate::error::NanowebError;
use crate::http::types::{Request, Response};
use crate::middleware::{BoxFuture, Middleware, Next};
use std::time::Instant;
use tracing::info;

pub struct RequestLogger;

impl Middleware for RequestLogger {
    fn handle<'a>(&'a self, req: Request, next: Next) -> BoxFuture<Result<Response, NanowebError>> {
        Box::pin(async move {
            let method = req.method.to_string();
            let path = req.path.clone();
            let remote = req.remote_addr.to_string();
            let request_id = req.request_id.clone();
            let start = Instant::now();

            let result = next.run(req).await;

            let elapsed = start.elapsed();
            let status = match &result {
                Ok(resp) => resp.status.0,
                Err(_) => 500,
            };

            info!(
                request_id = %request_id,
                method = %method,
                path = %path,
                remote = %remote,
                status = status,
                duration_ms = elapsed.as_millis(),
                "request completed"
            );

            result
        })
    }
}
