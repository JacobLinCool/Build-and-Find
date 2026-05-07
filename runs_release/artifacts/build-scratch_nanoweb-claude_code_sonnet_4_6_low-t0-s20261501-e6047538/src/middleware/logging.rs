use super::{Middleware, Next};
use crate::error::NanowebError;
use crate::http::types::{Request, Response};
use async_trait::async_trait;
use tracing::info;

pub struct LoggingMiddleware;

#[async_trait]
impl Middleware for LoggingMiddleware {
    async fn handle(&self, req: Request, next: Next<'_>) -> Result<Response, NanowebError> {
        let method = req.method.to_string();
        let path = req.path.clone();
        let start = std::time::Instant::now();

        let result = next.run(req).await;

        let elapsed = start.elapsed();
        match &result {
            Ok(resp) => info!(
                method = %method,
                path = %path,
                status = resp.status.code(),
                duration_ms = elapsed.as_millis(),
                "request completed"
            ),
            Err(e) => info!(
                method = %method,
                path = %path,
                error = %e,
                duration_ms = elapsed.as_millis(),
                "request error"
            ),
        }

        result
    }
}
