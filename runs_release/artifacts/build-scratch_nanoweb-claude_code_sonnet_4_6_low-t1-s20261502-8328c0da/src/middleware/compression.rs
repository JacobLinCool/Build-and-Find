use crate::error::NanowebError;
use crate::http::types::{Request, Response};
use crate::middleware::{BoxFuture, Middleware, Next};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;

pub struct GzipCompression {
    pub min_size: usize,
}

impl GzipCompression {
    pub fn new(min_size: usize) -> Self {
        GzipCompression { min_size }
    }
}

impl Default for GzipCompression {
    fn default() -> Self {
        GzipCompression::new(1024)
    }
}

impl Middleware for GzipCompression {
    fn handle<'a>(&'a self, req: Request, next: Next) -> BoxFuture<Result<Response, NanowebError>> {
        let min_size = self.min_size;
        let accepts_gzip = req
            .headers
            .get("accept-encoding")
            .map(|v| v.contains("gzip"))
            .unwrap_or(false);

        Box::pin(async move {
            let mut resp = next.run(req).await?;

            if accepts_gzip && resp.body.len() >= min_size {
                // Check content-encoding not already set
                if resp.headers.get("content-encoding").is_none() {
                    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
                    encoder
                        .write_all(&resp.body)
                        .map_err(|e| NanowebError::Io(e))?;
                    let compressed = encoder.finish().map_err(|e| NanowebError::Io(e))?;
                    resp.body = compressed;
                    resp.headers.set("content-encoding", "gzip");
                    resp.headers.set("content-length", resp.body.len().to_string());
                }
            }

            Ok(resp)
        })
    }
}
