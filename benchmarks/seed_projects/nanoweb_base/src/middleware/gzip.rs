//! Gzip response compression middleware backed by `flate2`.

use std::future::Future;
use std::pin::Pin;

use flate2::write::GzEncoder;
use flate2::Compression;

use crate::error::NanowebError;
use crate::http::types::{Request, Response};
use crate::middleware::{Middleware, MiddlewareOutcome};

/// Compression-level threshold (response body must exceed this many
/// bytes before it is worth compressing).
const MIN_COMPRESS_BYTES: usize = 256;

pub struct GzipMiddleware {
    pub level: u32,
    pub min_bytes: usize,
}

impl Default for GzipMiddleware {
    fn default() -> Self {
        Self {
            level: 6,
            min_bytes: MIN_COMPRESS_BYTES,
        }
    }
}

impl GzipMiddleware {
    pub fn new(level: u32) -> Self {
        Self {
            level,
            min_bytes: MIN_COMPRESS_BYTES,
        }
    }
}

fn request_accepts_gzip(req: &Request) -> bool {
    req.headers
        .get("accept-encoding")
        .map(|v| v.to_ascii_lowercase().contains("gzip"))
        .unwrap_or(false)
}

fn response_is_already_encoded(resp: &Response) -> bool {
    resp.headers.get("content-encoding").is_some()
}

fn compress(body: &[u8], level: u32) -> Result<Vec<u8>, NanowebError> {
    use std::io::Write;
    let mut enc = GzEncoder::new(Vec::new(), Compression::new(level));
    enc.write_all(body)
        .map_err(|e| NanowebError::Middleware(format!("gzip write: {}", e)))?;
    enc.finish()
        .map_err(|e| NanowebError::Middleware(format!("gzip finish: {}", e)))
}

impl Middleware for GzipMiddleware {
    fn name(&self) -> &'static str {
        "gzip"
    }

    fn before<'a>(
        &'a self,
        req: Request,
    ) -> Pin<Box<dyn Future<Output = Result<MiddlewareOutcome, NanowebError>> + Send + 'a>>
    {
        Box::pin(async move { Ok(MiddlewareOutcome::Continue(req)) })
    }

    fn after<'a>(
        &'a self,
        req: &'a Request,
        mut resp: Response,
    ) -> Pin<Box<dyn Future<Output = Result<Response, NanowebError>> + Send + 'a>> {
        Box::pin(async move {
            if !request_accepts_gzip(req) {
                return Ok(resp);
            }
            if response_is_already_encoded(&resp) {
                return Ok(resp);
            }
            if resp.body.len() < self.min_bytes {
                return Ok(resp);
            }
            let compressed = compress(&resp.body, self.level)?;
            resp.body = compressed;
            resp.headers.set("content-encoding", "gzip");
            resp.headers
                .set("content-length", resp.body.len().to_string());
            // Gzip varies responses by Accept-Encoding.
            match resp.headers.get("vary").map(|s| s.to_string()) {
                Some(existing) if !existing.to_ascii_lowercase().contains("accept-encoding") => {
                    resp.headers
                        .set("vary", format!("{}, Accept-Encoding", existing));
                }
                Some(_) => {}
                None => {
                    resp.headers.set("vary", "Accept-Encoding");
                }
            }
            Ok(resp)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::{Method, StatusCode};

    #[tokio::test(flavor = "current_thread")]
    async fn skips_when_client_wont_accept() {
        let m = GzipMiddleware::default();
        let req = Request::new(Method::Get, "/");
        let mut resp = Response::new(StatusCode::OK);
        resp.body = vec![b'a'; 1024];
        let out = m.after(&req, resp).await.unwrap();
        assert!(out.headers.get("content-encoding").is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn compresses_large_body() {
        let m = GzipMiddleware::default();
        let mut req = Request::new(Method::Get, "/");
        req.headers.set("accept-encoding", "gzip, deflate");
        let mut resp = Response::new(StatusCode::OK);
        resp.body = vec![b'a'; 4096];
        let out = m.after(&req, resp).await.unwrap();
        assert_eq!(out.headers.get("content-encoding"), Some("gzip"));
        assert!(out.body.len() < 4096);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn leaves_small_body_alone() {
        let m = GzipMiddleware::default();
        let mut req = Request::new(Method::Get, "/");
        req.headers.set("accept-encoding", "gzip");
        let resp = Response::text("tiny");
        let out = m.after(&req, resp).await.unwrap();
        assert!(out.headers.get("content-encoding").is_none());
    }
}
