use crate::error::NanowebError;
use crate::http::types::{Request, Response};
use crate::middleware::{BoxFuture, Middleware, Next};
use tracing::{info, warn};

pub struct LoggingMiddleware;

impl Middleware for LoggingMiddleware {
    fn call(&self, req: Request, next: Next) -> BoxFuture<Result<Response, NanowebError>> {
        Box::pin(async move {
            let method = req.method.to_string();
            let path = req.path.clone();
            let request_id = req.request_id.clone();
            let remote_addr = req.remote_addr.clone();

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
                        remote_addr = %remote_addr,
                        duration_ms = elapsed.as_millis(),
                        "request completed"
                    );
                }
                Err(e) => {
                    warn!(
                        request_id = %request_id,
                        method = %method,
                        path = %path,
                        error = %e,
                        remote_addr = %remote_addr,
                        duration_ms = elapsed.as_millis(),
                        "request failed"
                    );
                }
            }

            result
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::{HttpVersion, Method};
    use crate::middleware::{FnHandler, Next};
    use std::collections::HashMap;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_logging_passes_through() {
        let handler = Arc::new(FnHandler::new(|_req: Request| async {
            Ok(Response::text("hello"))
        }));
        let req = Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            query: None,
            version: HttpVersion::Http11,
            headers: vec![],
            body: vec![],
            params: HashMap::new(),
            remote_addr: "127.0.0.1".into(),
            request_id: "test-id".into(),
        };
        let mw: Vec<Arc<dyn Middleware>> = vec![Arc::new(LoggingMiddleware)];
        let next = Next::new(handler, mw);
        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status, 200);
    }
}
