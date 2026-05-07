use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;
use flate2::{Compression, write::GzEncoder};
use std::io::Write;

pub struct Gzip;

#[async_trait]
impl Middleware for Gzip {
    async fn after(&self, req: &Request, mut res: Response) -> Response {
        if !req.header("accept-encoding").unwrap_or("").contains("gzip") || res.body.len() < 32 {
            return res;
        }
        if res.headers.contains_key("Content-Encoding") {
            return res;
        }
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        if encoder.write_all(&res.body).is_ok() {
            if let Ok(body) = encoder.finish() {
                res.body = body;
                res.header("Content-Encoding", "gzip");
                res.header("Vary", "Accept-Encoding");
            }
        }
        res
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn construct() {
        let _ = super::Gzip;
    }
}
