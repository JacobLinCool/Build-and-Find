use super::Middleware;
use crate::http::{Request, Response};
use crate::NanowebError;
use async_trait::async_trait;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;

pub struct GzipCompression {
    pub min_size: usize,
}

impl Default for GzipCompression {
    fn default() -> Self {
        Self { min_size: 256 }
    }
}

#[async_trait]
impl Middleware for GzipCompression {
    async fn before(&self, _req: &mut Request) -> Result<Option<Response>, NanowebError> {
        Ok(None)
    }
    async fn after(&self, req: &Request, mut resp: Response) -> Result<Response, NanowebError> {
        let accept = req
            .header("accept-encoding")
            .unwrap_or("")
            .to_ascii_lowercase();
        if !accept.contains("gzip") {
            return Ok(resp);
        }
        if resp.body.len() < self.min_size {
            return Ok(resp);
        }
        if resp.headers.get("content-encoding").is_some() {
            return Ok(resp);
        }
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&resp.body).map_err(NanowebError::Io)?;
        let compressed = enc.finish().map_err(NanowebError::Io)?;
        resp.body = compressed;
        resp.headers.insert("Content-Encoding", "gzip");
        resp.headers.insert("Vary", "Accept-Encoding");
        resp.headers.remove("Content-Length");
        Ok(resp)
    }
}
