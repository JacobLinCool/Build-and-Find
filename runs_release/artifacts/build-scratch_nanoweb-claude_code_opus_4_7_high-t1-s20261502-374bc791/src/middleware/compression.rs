//! Response gzip compression middleware.
//!
//! Compresses responses when the client advertises gzip support via
//! `Accept-Encoding`, the response has a textual or compressible body, and
//! is large enough to be worth compressing.

use crate::error::{NanowebError, Result};
use crate::http::{Request, Response};

use super::{BoxFuture, Middleware, Next};

pub struct GzipMiddleware {
    pub min_size: usize,
}

impl GzipMiddleware {
    pub fn new() -> Self {
        Self { min_size: 256 }
    }

    pub fn with_min_size(min_size: usize) -> Self {
        Self { min_size }
    }
}

impl Default for GzipMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

fn client_accepts_gzip(req: &Request) -> bool {
    let Some(ae) = req.header("accept-encoding") else {
        return false;
    };
    ae.split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .any(|s| s.starts_with("gzip"))
}

fn already_encoded(resp: &Response) -> bool {
    resp.headers.get("content-encoding").is_some()
}

fn is_compressible(resp: &Response) -> bool {
    let Some(ct) = resp.headers.get("content-type") else {
        return false;
    };
    let ct = ct.to_ascii_lowercase();
    ct.starts_with("text/")
        || ct.starts_with("application/json")
        || ct.starts_with("application/javascript")
        || ct.starts_with("application/xml")
        || ct.starts_with("application/xhtml")
}

impl Middleware for GzipMiddleware {
    fn handle<'a>(
        &'a self,
        req: &'a mut Request,
        next: Next<'a>,
    ) -> BoxFuture<'a, Result<Response>> {
        Box::pin(async move {
            let accepts = client_accepts_gzip(req);
            let mut resp = next.run(req).await?;
            if !accepts || already_encoded(&resp) || resp.body.len() < self.min_size {
                return Ok(resp);
            }
            if !is_compressible(&resp) {
                return Ok(resp);
            }
            let compressed = gzip_encode(&resp.body)?;
            resp.body = compressed;
            resp.headers.insert("content-encoding", "gzip");
            resp.headers.insert("vary", "accept-encoding");
            // Reset content-length so it gets recomputed.
            resp.headers.remove("content-length");
            Ok(resp)
        })
    }
}

fn gzip_encode(data: &[u8]) -> Result<Vec<u8>> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .map_err(|e| NanowebError::Internal(format!("gzip: {}", e)))?;
    encoder
        .finish()
        .map_err(|e| NanowebError::Internal(format!("gzip: {}", e)))
}
