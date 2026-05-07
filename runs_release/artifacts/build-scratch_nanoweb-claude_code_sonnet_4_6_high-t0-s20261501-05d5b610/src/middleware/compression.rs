use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write as IoWrite;

use crate::error::NanowebError;
use crate::http::{Request, Response};
use crate::middleware::{Middleware, Next};

pub struct CompressionMiddleware {
    pub min_size: usize,
}

impl CompressionMiddleware {
    pub fn new() -> Self {
        Self { min_size: 1024 }
    }

    pub fn with_min_size(min_size: usize) -> Self {
        Self { min_size }
    }
}

impl Default for CompressionMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Middleware for CompressionMiddleware {
    async fn handle(&self, req: Request, next: Next<'_>) -> Result<Response, NanowebError> {
        let accepts_gzip = req
            .headers
            .get("accept-encoding")
            .map(|ae| ae.to_lowercase().contains("gzip"))
            .unwrap_or(false);

        let mut response = next.run(req).await?;

        if accepts_gzip && response.body.len() >= self.min_size {
            let ct = response
                .headers
                .get("content-type")
                .unwrap_or("")
                .to_string();
            // Only compress text-like content
            if is_compressible(&ct) {
                match gzip_compress(&response.body) {
                    Ok(compressed) => {
                        response.headers.insert("Content-Encoding", "gzip");
                        response
                            .headers
                            .insert("Content-Length", compressed.len().to_string());
                        response.headers.insert("Vary", "Accept-Encoding");
                        response.body = compressed;
                    }
                    Err(_) => {} // Fall through without compression
                }
            }
        }

        Ok(response)
    }
}

fn is_compressible(content_type: &str) -> bool {
    let ct = content_type.to_lowercase();
    ct.contains("text/")
        || ct.contains("application/json")
        || ct.contains("application/javascript")
        || ct.contains("application/xml")
        || ct.contains("image/svg")
}

fn gzip_compress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    encoder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Headers, Method, Version};
    use crate::middleware::MiddlewareChain;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_compresses_when_accepted() {
        let mw = Arc::new(CompressionMiddleware::with_min_size(0));
        let body = "hello world this is some text content that will be compressed";
        let handler = Arc::new(move |_req: Request| -> crate::middleware::BoxFuture<_> {
            let b = body.to_string();
            Box::pin(async move { Ok(Response::ok().with_text(b)) })
        });
        let chain = MiddlewareChain::new(vec![mw], handler);
        let mut req = Request::new(
            Method::Get,
            "/".to_string(),
            Version::Http11,
            Headers::new(),
            Vec::new(),
            None,
            "id".to_string(),
        );
        req.headers.insert("Accept-Encoding", "gzip");
        let resp = chain.run(req).await.unwrap();
        assert_eq!(resp.headers.get("content-encoding"), Some("gzip"));
    }

    #[tokio::test]
    async fn test_no_compression_without_accept() {
        let mw = Arc::new(CompressionMiddleware::with_min_size(0));
        let handler = Arc::new(|_req: Request| -> crate::middleware::BoxFuture<_> {
            Box::pin(async { Ok(Response::ok().with_text("hello")) })
        });
        let chain = MiddlewareChain::new(vec![mw], handler);
        let req = Request::new(
            Method::Get,
            "/".to_string(),
            Version::Http11,
            Headers::new(),
            Vec::new(),
            None,
            "id".to_string(),
        );
        let resp = chain.run(req).await.unwrap();
        assert!(resp.headers.get("content-encoding").is_none());
    }
}
