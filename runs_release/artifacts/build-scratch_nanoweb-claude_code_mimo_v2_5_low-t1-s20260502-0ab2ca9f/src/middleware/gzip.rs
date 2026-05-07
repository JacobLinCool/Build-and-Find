use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;

use crate::http::{Request, Response};
use crate::middleware::Middleware;

pub struct GzipMiddleware;

impl Middleware for GzipMiddleware {
    fn after(&self, req: &Request, res: &mut Response) {
        let accepts = req.headers.get("accept-encoding").unwrap_or("");
        if !accepts.contains("gzip") {
            return;
        }
        if res.body.len() < 256 {
            return;
        }
        if res.headers.get("content-encoding").is_some() {
            return;
        }

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        if encoder.write_all(&res.body).is_ok() {
            if let Ok(compressed) = encoder.finish() {
                res.body = compressed;
                res.headers.insert("content-encoding".into(), "gzip".into());
                res.headers
                    .insert("content-length".into(), res.body.len().to_string());
            }
        }
    }
}

impl GzipMiddleware {
    pub fn new() -> Self {
        GzipMiddleware
    }
}
