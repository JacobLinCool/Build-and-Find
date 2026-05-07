use std::time::Instant;

use async_trait::async_trait;
use tracing::{info, warn};

use crate::error::Result;
use crate::http::{Request, Response};

use super::{Middleware, Next};

#[derive(Default)]
pub struct RequestLogging;

#[async_trait]
impl Middleware for RequestLogging {
    async fn handle(&self, request: Request, next: Next<'_>) -> Result<Response> {
        let started = Instant::now();
        let request_id = request.request_id.clone();
        let method = request.method.to_string();
        let path = request.path.clone();
        let remote = request.remote_addr.map(|addr| addr.to_string());
        let response = next.run(request).await;
        match &response {
            Ok(response) => {
                info!(
                    request_id = %request_id,
                    method = %method,
                    path = %path,
                    remote = remote.as_deref().unwrap_or("-"),
                    status = response.status.as_u16(),
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "request completed"
                );
            }
            Err(err) => {
                warn!(
                    request_id = %request_id,
                    method = %method,
                    path = %path,
                    remote = remote.as_deref().unwrap_or("-"),
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    error = %err,
                    "request failed"
                );
            }
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Method, StatusCode, Version};
    use crate::router::Handler;
    use std::sync::Arc;

    struct OkHandler;

    #[async_trait]
    impl Handler for OkHandler {
        async fn handle(&self, _request: Request) -> Result<Response> {
            Ok(Response::text(StatusCode::Ok, "ok"))
        }
    }

    #[tokio::test]
    async fn logs_and_passes_through() {
        let request = Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            query: None,
            version: Version::Http11,
            headers: Default::default(),
            body: vec![],
            params: Default::default(),
            remote_addr: None,
            request_id: "req".into(),
            session: None,
        };
        let response =
            crate::middleware::execute(&[Arc::new(RequestLogging)], Arc::new(OkHandler), request)
                .await
                .unwrap();
        assert_eq!(response.status, StatusCode::Ok);
    }
}
