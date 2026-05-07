use super::MiddlewareDyn;
use crate::http::{Request, Response};
use std::future::Future;
use std::pin::Pin;
use std::time::Instant;

/// Structured request logging middleware.
pub struct LoggingMiddleware;

impl LoggingMiddleware {
    pub fn new() -> Self {
        Self
    }
}

impl MiddlewareDyn for LoggingMiddleware {
    fn before<'a>(
        &'a self,
        mut req: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Request, Response>> + Send + 'a>> {
        Box::pin(async move {
            tracing::info!(
                request_id = %req.request_id,
                method = %req.method,
                path = %req.path,
                "incoming request"
            );
            // Store start time in a header for duration tracking
            let start = Instant::now();
            req.headers
                .insert("x-start-time", format!("{:?}", start.elapsed()));
            // We store the start time as a nanosecond timestamp
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            req.headers.insert("x-request-start", nanos.to_string());
            Ok(req)
        })
    }

    fn after<'a>(
        &'a self,
        req: &'a Request,
        resp: Response,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        Box::pin(async move {
            let start_nanos: u128 = req
                .header("x-request-start")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let now_nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let duration_ms = (now_nanos.saturating_sub(start_nanos)) / 1_000_000;

            tracing::info!(
                request_id = %req.request_id,
                method = %req.method,
                path = %req.path,
                status = resp.status.0,
                duration_ms = duration_ms,
                "request completed"
            );
            resp
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HeaderMap, Method, StatusCode};

    #[tokio::test]
    async fn test_logging_middleware() {
        let mw = LoggingMiddleware::new();
        let req = Request {
            method: Method::Get,
            target: "/test".into(),
            path: "/test".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderMap::new(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: "test-123".into(),
            session_data: Default::default(),
        };

        let req = mw.before(req).await.unwrap();
        assert!(req.header("x-request-start").is_some());

        let resp = Response::ok();
        let resp = mw.after(&req, resp).await;
        assert_eq!(resp.status, StatusCode::OK);
    }
}
