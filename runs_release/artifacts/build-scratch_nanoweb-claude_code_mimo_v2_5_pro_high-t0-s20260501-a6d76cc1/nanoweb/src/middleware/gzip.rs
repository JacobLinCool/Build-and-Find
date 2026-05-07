use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;

pub struct GzipMiddleware;

#[async_trait]
impl Middleware for GzipMiddleware {
    async fn after(&self, req: &Request, resp: &mut Response) {
        let accepts_gzip = req
            .header("accept-encoding")
            .map(|v| v.contains("gzip"))
            .unwrap_or(false);

        if !accepts_gzip || resp.body.is_empty() {
            return;
        }

        let content_type = resp
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.as_str())
            .unwrap_or("");

        // Only compress text-like content
        if !(content_type.contains("text/")
            || content_type.contains("application/json")
            || content_type.contains("application/javascript")
            || content_type.contains("application/xml"))
        {
            return;
        }

        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        if encoder.write_all(&resp.body).is_err() {
            return;
        }
        match encoder.finish() {
            Ok(compressed) if compressed.len() < resp.body.len() => {
                resp.body = compressed;
                // Remove existing content-length
                resp.headers
                    .retain(|(k, _)| !k.eq_ignore_ascii_case("content-length"));
                resp.headers
                    .push(("content-length".into(), resp.body.len().to_string()));
                resp.headers
                    .push(("content-encoding".into(), "gzip".into()));
                resp.headers.push(("vary".into(), "accept-encoding".into()));
            }
            _ => {}
        }
    }
}
