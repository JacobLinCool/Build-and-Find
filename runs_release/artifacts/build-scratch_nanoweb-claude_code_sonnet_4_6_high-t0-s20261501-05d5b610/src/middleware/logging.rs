use crate::error::NanowebError;
use crate::http::{Request, Response};
use crate::middleware::{Middleware, Next};
use tracing::{info, warn};

pub struct LoggingMiddleware;

#[async_trait::async_trait]
impl Middleware for LoggingMiddleware {
    async fn handle(&self, req: Request, next: Next<'_>) -> Result<Response, NanowebError> {
        let method = req.method.as_str().to_string();
        let path = req.path.clone();
        let request_id = req.request_id.clone();
        let remote = req
            .remote_addr
            .map(|a| a.to_string())
            .unwrap_or_else(|| "unknown".into());

        let start = std::time::Instant::now();
        let result = next.run(req).await;
        let elapsed = start.elapsed();

        match &result {
            Ok(resp) => {
                info!(
                    request_id = %request_id,
                    method = %method,
                    path = %path,
                    status = resp.status,
                    remote = %remote,
                    latency_ms = elapsed.as_millis(),
                    "request completed"
                );
            }
            Err(e) => {
                warn!(
                    request_id = %request_id,
                    method = %method,
                    path = %path,
                    error = %e,
                    remote = %remote,
                    latency_ms = elapsed.as_millis(),
                    "request failed"
                );
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Headers, Method, Version};
    use crate::middleware::MiddlewareChain;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_logging_passes_through() {
        let mw = Arc::new(LoggingMiddleware);
        let handler = Arc::new(|_req: Request| -> crate::middleware::BoxFuture<_> {
            Box::pin(async { Ok(Response::ok().with_text("logged")) })
        });
        let chain = MiddlewareChain::new(vec![mw], handler);
        let req = Request::new(
            Method::Get,
            "/".to_string(),
            Version::Http11,
            Headers::new(),
            Vec::new(),
            None,
            "test-id".to_string(),
        );
        let resp = chain.run(req).await.unwrap();
        assert_eq!(resp.status, 200);
    }
}
