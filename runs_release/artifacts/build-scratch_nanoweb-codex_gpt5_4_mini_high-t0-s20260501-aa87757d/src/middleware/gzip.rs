use crate::{
    app::AppState,
    error::Result,
    http::types::{Request, Response},
    middleware::{Middleware, Next},
};
use async_trait::async_trait;
use flate2::{write::GzEncoder, Compression};
use std::{io::Write, sync::Arc};

pub struct GzipMiddleware;

#[async_trait]
impl Middleware for GzipMiddleware {
    async fn handle(&self, req: Request, ctx: Arc<AppState>, next: Next<'_>) -> Result<Response> {
        let accept_gzip = req
            .header("accept-encoding")
            .map(|value| value.contains("gzip"))
            .unwrap_or(false);
        let mut response = next.run(req).await?;
        if !accept_gzip
            || response.header("content-encoding").is_some()
            || response.body.is_empty()
            || matches!(response.status.as_u16(), 204 | 304)
        {
            return Ok(response);
        }
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&response.body).expect("gzip write");
        let compressed = encoder.finish().expect("gzip finish");
        response.body = compressed;
        response.set_header("content-encoding", "gzip");
        response.set_header("vary", "Accept-Encoding");
        response.set_header("content-length", response.body.len().to_string());
        let _ = ctx;
        Ok(response)
    }
}
