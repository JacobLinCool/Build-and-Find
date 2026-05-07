use super::{Middleware, Next};
use crate::error::NanowebError;
use crate::http::types::{Request, Response};
use async_trait::async_trait;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;

pub struct GzipMiddleware {
    pub min_size: usize,
}

impl Default for GzipMiddleware {
    fn default() -> Self {
        GzipMiddleware { min_size: 1024 }
    }
}

#[async_trait]
impl Middleware for GzipMiddleware {
    async fn handle(&self, req: Request, next: Next<'_>) -> Result<Response, NanowebError> {
        let accepts_gzip = req
            .header("accept-encoding")
            .map(|v| v.contains("gzip"))
            .unwrap_or(false);

        let mut resp = next.run(req).await?;

        if accepts_gzip && resp.body.len() >= self.min_size {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder
                .write_all(&resp.body)
                .map_err(|e| NanowebError::InternalServerError(e.to_string()))?;
            let compressed = encoder
                .finish()
                .map_err(|e| NanowebError::InternalServerError(e.to_string()))?;

            resp.body = compressed;
            resp.headers
                .insert("content-encoding".to_string(), "gzip".to_string());
            resp.headers
                .insert("content-length".to_string(), resp.body.len().to_string());
        }

        Ok(resp)
    }
}
