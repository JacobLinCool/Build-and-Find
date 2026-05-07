use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::error::Result;
use crate::http::{Request, Response};
use crate::middleware::Middleware;

pub struct RequestLogging;

#[async_trait]
impl Middleware for RequestLogging {
    async fn before(&self, request: &mut Request) -> Result<Option<Response>> {
        request.set_header("x-nanoweb-start", format!("{:?}", Instant::now()));
        info!(
            request_id = %request.request_id,
            method = %request.method.as_str(),
            path = %request.path,
            "request_started"
        );
        Ok(None)
    }

    async fn after(&self, request: &Request, response: &mut Response) -> Result<()> {
        info!(
            request_id = %request.request_id,
            method = %request.method.as_str(),
            path = %request.path,
            status = response.status.0,
            "request_finished"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Method, StatusCode};

    #[tokio::test]
    async fn logging_is_passthrough() {
        let mut req = Request::new(Method::Get, "/");
        assert!(RequestLogging.before(&mut req).await.unwrap().is_none());
        let mut res = Response::text(StatusCode::OK, "ok");
        RequestLogging.after(&req, &mut res).await.unwrap();
    }
}
