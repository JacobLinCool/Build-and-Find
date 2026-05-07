use super::Middleware;
use crate::http::{Request, Response};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;

pub struct GzipMiddleware {
    min_size: usize,
}

impl GzipMiddleware {
    pub fn new() -> Self {
        Self { min_size: 256 }
    }

    pub fn with_min_size(mut self, min_size: usize) -> Self {
        self.min_size = min_size;
        self
    }
}

impl Middleware for GzipMiddleware {
    fn after(&self, req: &Request, resp: &mut Response) {
        let accept_encoding = req.header("accept-encoding").unwrap_or("");

        if !accept_encoding.contains("gzip") {
            return;
        }

        let has_encoding = resp
            .headers
            .iter()
            .any(|(k, _)| k.to_ascii_lowercase() == "content-encoding");
        if has_encoding {
            return;
        }

        let is_compressible = resp
            .headers
            .iter()
            .find(|(k, _)| k.to_ascii_lowercase() == "content-type")
            .map(|(_, v)| {
                let v = v.to_ascii_lowercase();
                v.starts_with("text/")
                    || v.contains("json")
                    || v.contains("xml")
                    || v.contains("javascript")
                    || v.contains("css")
                    || v.contains("html")
            })
            .unwrap_or(false);

        if resp.body.len() < self.min_size || !is_compressible {
            return;
        }

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        if encoder.write_all(&resp.body).is_ok() {
            if let Ok(compressed) = encoder.finish() {
                if compressed.len() < resp.body.len() {
                    resp.body = compressed;
                    resp.set_header("content-encoding", "gzip");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_gzip_compression() {
        let mw = GzipMiddleware::new();
        let mut req = Request::new(crate::http::Method::Get, "/".into(), "HTTP/1.1".into());
        req.headers
            .insert("accept-encoding".into(), "gzip, deflate".into());

        let mut resp = Response::ok()
            .with_header("content-type", "text/html")
            .with_body("hello world ".repeat(100));

        mw.after(&req, &mut resp);
        assert!(resp
            .headers
            .iter()
            .any(|(k, v)| k == "content-encoding" && v == "gzip"));
    }

    #[tokio::test]
    async fn test_gzip_no_encoding_header() {
        let mw = GzipMiddleware::new();
        let req = Request::new(crate::http::Method::Get, "/".into(), "HTTP/1.1".into());

        let mut resp = Response::ok()
            .with_header("content-type", "text/html")
            .with_body("hello world ".repeat(100));

        mw.after(&req, &mut resp);
        assert!(!resp.headers.iter().any(|(k, _)| k == "content-encoding"));
    }
}
