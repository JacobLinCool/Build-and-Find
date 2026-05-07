use async_trait::async_trait;
use flate2::Compression;
use flate2::write::GzEncoder;
use std::io::Write;

use crate::error::Result;
use crate::http::{Request, Response};
use crate::middleware::Middleware;

pub struct GzipCompression;

#[async_trait]
impl Middleware for GzipCompression {
    async fn after(&self, request: &Request, response: &mut Response) -> Result<()> {
        if response.body.is_empty()
            || response.header("content-encoding").is_some()
            || !request
                .header("accept-encoding")
                .unwrap_or("")
                .split(',')
                .any(|v| v.trim().eq_ignore_ascii_case("gzip"))
        {
            return Ok(());
        }
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&response.body)?;
        response.body = encoder.finish()?;
        response.set_header("Content-Encoding", "gzip");
        response.set_header("Vary", "Accept-Encoding");
        response.set_header("Content-Length", response.body.len().to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Method, StatusCode};

    #[tokio::test]
    async fn compresses_when_accepted() {
        let mut req = Request::new(Method::Get, "/");
        req.set_header("Accept-Encoding", "gzip");
        let mut res = Response::text(StatusCode::OK, "hello");
        GzipCompression.after(&req, &mut res).await.unwrap();
        assert_eq!(res.header("content-encoding"), Some("gzip"));
        assert_ne!(res.body, b"hello");
    }
}
