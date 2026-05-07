use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;

pub struct Gzip {
    pub min_size: usize,
}

impl Gzip {
    pub fn new() -> Self {
        Self { min_size: 256 }
    }
}

impl Default for Gzip {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for Gzip {
    async fn after(&self, req: &Request, res: &mut Response) {
        if res.body.len() < self.min_size {
            return;
        }
        if res.headers.contains("Content-Encoding") {
            return;
        }
        let accept = req.header("Accept-Encoding").unwrap_or("");
        if !accept
            .split(',')
            .any(|s| s.trim().eq_ignore_ascii_case("gzip"))
        {
            return;
        }
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        if enc.write_all(&res.body).is_err() {
            return;
        }
        let compressed = match enc.finish() {
            Ok(v) => v,
            Err(_) => return,
        };
        res.body = compressed;
        res.headers.insert("Content-Encoding", "gzip");
        res.headers.remove("Content-Length");
        res.headers
            .insert("Content-Length", res.body.len().to_string());
        // Vary
        let vary = res.headers.get("Vary").map(|s| s.to_string());
        match vary {
            Some(v) if !v.contains("Accept-Encoding") => {
                res.headers
                    .insert("Vary", format!("{}, Accept-Encoding", v));
            }
            None => {
                res.headers.insert("Vary", "Accept-Encoding");
            }
            _ => {}
        }
    }
}
