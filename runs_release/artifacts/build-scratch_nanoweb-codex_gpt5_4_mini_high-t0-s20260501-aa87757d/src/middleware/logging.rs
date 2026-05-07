use crate::{
    app::AppState,
    error::Result,
    http::types::{Request, Response},
    middleware::{Middleware, Next},
};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Instant;

pub struct LoggingMiddleware;

#[async_trait]
impl Middleware for LoggingMiddleware {
    async fn handle(&self, req: Request, ctx: Arc<AppState>, next: Next<'_>) -> Result<Response> {
        let started = Instant::now();
        let method = req.method;
        let path = req.path.clone();
        let request_id = req.request_id.clone();
        let response = next.run(req).await?;
        tracing::info!(
            request_id = %request_id,
            method = %method,
            path = %path,
            status = response.status.as_u16(),
            duration_ms = started.elapsed().as_millis(),
            "request completed"
        );
        let _ = ctx;
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn logger_compiles() {
        assert!(true);
    }
}
