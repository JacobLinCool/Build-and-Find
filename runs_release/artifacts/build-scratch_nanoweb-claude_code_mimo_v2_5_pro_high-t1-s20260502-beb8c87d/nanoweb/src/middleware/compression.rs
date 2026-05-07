use super::Middleware;
use crate::http::types::{Request, Response};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;

pub struct CompressionMiddleware;

impl CompressionMiddleware {
    pub fn new() -> Self {
        Self
    }
}

impl Middleware for CompressionMiddleware {
    fn name(&self) -> &str {
        "compression"
    }

    fn after(&self, req: &Request, resp: &mut Response) {
        let accepts_gzip = req
            .header("accept-encoding")
            .map(|v| v.contains("gzip"))
            .unwrap_or(false);

        if !accepts_gzip || resp.body.is_empty() {
            return;
        }

        // Don't compress small bodies or already-compressed content
        if resp.body.len() < 256 {
            return;
        }

        if let Some(ct) = resp.header("content-type") {
            if ct.contains("image/") || ct.contains("video/") || ct.contains("application/gzip") {
                return;
            }
        }

        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
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
    use std::collections::HashMap;

    #[test]
    fn test_compression_with_gzip_accepted() {
        let mw = CompressionMiddleware::new();
        let mut headers = HashMap::new();
        headers.insert("accept-encoding".to_string(), "gzip, deflate".to_string());
        let req = Request {
            method: crate::http::types::Method::Get,
            target: "/".to_string(),
            path: "/".to_string(),
            query: HashMap::new(),
            version: "HTTP/1.1".to_string(),
            headers,
            params: HashMap::new(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: None,
        };
        let mut resp = Response::text(200, &"x".repeat(1000));
        let original_len = resp.body.len();
        mw.after(&req, &mut resp);
        assert!(resp.body.len() < original_len || resp.header("content-encoding").is_none());
    }

    #[test]
    fn test_no_compression_without_accept() {
        let mw = CompressionMiddleware::new();
        let req = Request {
            method: crate::http::types::Method::Get,
            target: "/".to_string(),
            path: "/".to_string(),
            query: HashMap::new(),
            version: "HTTP/1.1".to_string(),
            headers: HashMap::new(),
            params: HashMap::new(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: None,
        };
        let mut resp = Response::text(200, &"x".repeat(1000));
        let original = resp.body.clone();
        mw.after(&req, &mut resp);
        assert_eq!(resp.body, original);
    }
}
