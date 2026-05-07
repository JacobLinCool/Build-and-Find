use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;
use flate2::{Compression, write::GzEncoder};
use std::io::Write;

#[derive(Clone)]
pub struct Gzip {
    min_size: usize,
}

impl Gzip {
    pub fn new() -> Self {
        Self { min_size: 128 }
    }
}

impl Default for Gzip {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for Gzip {
    async fn after(&self, request: &Request, response: &mut Response) {
        if response.body.len() < self.min_size
            || response.header("content-encoding").is_some()
            || !request
                .header("accept-encoding")
                .unwrap_or("")
                .split(',')
                .any(|v| v.trim() == "gzip")
        {
            return;
        }
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        if encoder.write_all(&response.body).is_ok() {
            if let Ok(body) = encoder.finish() {
                response.body = body;
                response.set_header("Content-Encoding", "gzip");
                response.set_header("Vary", "Accept-Encoding");
                response.set_header("Content-Length", response.body.len().to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn compresses_when_accepted() {
        let mut req = Request::new(crate::http::Method::Get, "/");
        req.set_header("Accept-Encoding", "gzip");
        let mut res = Response::text(200, "x".repeat(200));
        Gzip::new().after(&req, &mut res).await;
        assert_eq!(res.header("content-encoding"), Some("gzip"));
    }
}
