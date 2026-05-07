use super::Middleware;
use crate::http::{Request, Response};
use std::future::Future;
use std::pin::Pin;
use tracing::info;

pub struct LoggingMiddleware;

impl LoggingMiddleware {
    pub fn new() -> Self {
        Self
    }
}

impl Middleware for LoggingMiddleware {
    fn before(
        &self,
        req: &mut Request,
    ) -> Pin<Box<dyn Future<Output = Option<Response>> + Send + 'static>> {
        let method = req.method.to_string();
        let path = req.path.clone();
        let request_id = req.request_id.clone();
        let remote = req.remote_addr.map(|a| a.to_string()).unwrap_or_default();

        Box::pin(async move {
            info!(
                method = %method,
                path = %path,
                request_id = %request_id,
                remote = %remote,
                "Request started"
            );
            None
        })
    }

    fn after(&self, req: &Request, resp: &mut Response) {
        let method = req.method.to_string();
        let path = req.path.clone();
        let request_id = req.request_id.clone();
        let status = resp.status;
        let remote = req.remote_addr.map(|a| a.to_string()).unwrap_or_default();

        info!(
            method = %method,
            path = %path,
            request_id = %request_id,
            status = status,
            remote = %remote,
            "Request completed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_logging_middleware() {
        let mw = LoggingMiddleware::new();
        let mut req = Request::new(crate::http::Method::Get, "/test".into(), "HTTP/1.1".into());
        req.request_id = "test-123".into();

        let result = mw.before(&mut req).await;
        assert!(result.is_none());

        let mut resp = Response::ok();
        mw.after(&req, &mut resp);
    }
}
