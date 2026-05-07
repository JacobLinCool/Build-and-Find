use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;
use tracing::info;

#[derive(Default)]
pub struct RequestLogger;

#[async_trait]
impl Middleware for RequestLogger {
    async fn before(&self, request: &mut Request) -> Option<Response> {
        info!(
            request_id = %request.request_id,
            method = %request.method,
            path = %request.path,
            "request started"
        );
        None
    }

    async fn after(&self, request: &Request, response: &mut Response) {
        info!(
            request_id = %request.request_id,
            status = response.status,
            "request completed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn logger_passes_through() {
        let mut req = Request::new(crate::http::Method::Get, "/");
        assert!(RequestLogger.before(&mut req).await.is_none());
    }
}
