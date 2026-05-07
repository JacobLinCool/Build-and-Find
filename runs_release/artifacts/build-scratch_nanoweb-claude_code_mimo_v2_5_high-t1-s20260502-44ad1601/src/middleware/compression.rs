use crate::http::request::Request;
use crate::http::response::Response;
use crate::middleware::Middleware;

pub struct CompressionMiddleware;

impl CompressionMiddleware {
    pub fn new() -> Self {
        CompressionMiddleware
    }
}

impl Default for CompressionMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl Middleware for CompressionMiddleware {
    fn handle(&self, req: Request, next: Box<dyn FnOnce(Request) -> Response + Send>) -> Response {
        let accept_gzip = req.accept_encoding().iter().any(|e| e.contains("gzip"));

        let mut resp = next(req);

        if accept_gzip && resp.body.len() > 100 && !resp.headers.contains_key("content-encoding") {
            use std::io::Write;
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            if encoder.write_all(&resp.body).is_ok() {
                if let Ok(compressed) = encoder.finish() {
                    resp.body = compressed;
                    resp.headers
                        .insert("content-encoding".into(), "gzip".into());
                }
            }
        }

        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_compression_without_accept() {
        let mw = CompressionMiddleware::new();
        let req = Request {
            method: crate::http::parser::Method::Get,
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: std::collections::HashMap::new(),
            body: Vec::new(),
            params: std::collections::HashMap::new(),
            query: std::collections::HashMap::new(),
            remote_addr: None,
            request_id: "test".into(),
        };
        let resp = mw.handle(req, Box::new(|_| Response::text("short")));
        assert!(!resp.headers.contains_key("content-encoding"));
    }
}
