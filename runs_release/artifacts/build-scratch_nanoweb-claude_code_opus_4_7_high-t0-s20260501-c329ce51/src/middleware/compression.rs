use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;
use flate2::write::GzEncoder;
use flate2::Compression as GzLevel;
use std::io::Write;

pub struct Compression {
    pub min_size: usize,
}

impl Compression {
    pub fn new() -> Self {
        Self { min_size: 256 }
    }
    pub fn with_min_size(min_size: usize) -> Self {
        Self { min_size }
    }
}

impl Default for Compression {
    fn default() -> Self {
        Self::new()
    }
}

fn accepts_gzip(req: &Request) -> bool {
    req.header("Accept-Encoding")
        .map(|s| {
            s.split(',')
                .map(|e| e.trim())
                .any(|e| e.eq_ignore_ascii_case("gzip") || e.starts_with("gzip;"))
        })
        .unwrap_or(false)
}

fn already_encoded(resp: &Response) -> bool {
    resp.headers.get("Content-Encoding").is_some()
}

#[async_trait]
impl Middleware for Compression {
    fn name(&self) -> &'static str {
        "compression"
    }

    async fn before(&self, req: &mut Request) -> Option<Response> {
        // store the accepts flag in extensions for after() to consult
        req.extensions.insert(
            "compression.accept_gzip",
            if accepts_gzip(req) { "1" } else { "0" },
        );
        None
    }

    async fn after(&self, req: &Request, resp: &mut Response) {
        if already_encoded(resp) {
            return;
        }
        let accepts = req
            .extensions
            .get("compression.accept_gzip")
            .map(|v| v == "1")
            .unwrap_or_else(|| accepts_gzip(req));
        if !accepts {
            return;
        }
        if resp.body.len() < self.min_size {
            return;
        }
        let mut enc = GzEncoder::new(Vec::new(), GzLevel::default());
        if enc.write_all(&resp.body).is_err() {
            return;
        }
        match enc.finish() {
            Ok(buf) => {
                resp.body = buf;
                resp.headers.insert("Content-Encoding", "gzip");
                resp.headers.insert("Vary", "Accept-Encoding");
            }
            Err(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Method, Version};

    #[tokio::test]
    async fn compresses_when_accepted() {
        let mw = Compression::with_min_size(0);
        let mut req = Request::new(Method::Get, "/".into(), Version::Http11);
        req.headers.insert("Accept-Encoding", "gzip");
        let _ = mw.before(&mut req).await;
        let mut resp = Response::text("hello world".repeat(100));
        mw.after(&req, &mut resp).await;
        assert_eq!(resp.headers.get("Content-Encoding"), Some("gzip"));
    }

    #[tokio::test]
    async fn skips_without_accept() {
        let mw = Compression::new();
        let mut req = Request::new(Method::Get, "/".into(), Version::Http11);
        let _ = mw.before(&mut req).await;
        let mut resp = Response::text("hello".repeat(100));
        mw.after(&req, &mut resp).await;
        assert!(resp.headers.get("Content-Encoding").is_none());
    }
}
