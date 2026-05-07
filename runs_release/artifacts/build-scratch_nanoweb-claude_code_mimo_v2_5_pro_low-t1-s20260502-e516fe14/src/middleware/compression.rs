use super::MiddlewareDyn;
use crate::http::{Request, Response};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::future::Future;
use std::io::Write;
use std::pin::Pin;

/// Gzip response compression middleware.
pub struct CompressionMiddleware {
    min_size: usize,
}

impl CompressionMiddleware {
    pub fn new() -> Self {
        Self { min_size: 256 }
    }

    pub fn with_min_size(min_size: usize) -> Self {
        Self { min_size }
    }
}

impl MiddlewareDyn for CompressionMiddleware {
    fn before<'a>(
        &'a self,
        req: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Request, Response>> + Send + 'a>> {
        Box::pin(async move { Ok(req) })
    }

    fn after<'a>(
        &'a self,
        req: &'a Request,
        mut resp: Response,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        Box::pin(async move {
            let accepts_gzip = req
                .header("accept-encoding")
                .map(|v| v.contains("gzip"))
                .unwrap_or(false);

            if !accepts_gzip || resp.body.len() < self.min_size {
                return resp;
            }

            // Don't compress if already compressed
            if resp.headers.get("content-encoding").is_some() {
                return resp;
            }

            match gzip_compress(&resp.body) {
                Ok(compressed) => {
                    if compressed.len() < resp.body.len() {
                        resp.body = compressed;
                        resp.headers.insert("content-encoding", "gzip");
                        resp.headers
                            .insert("content-length", resp.body.len().to_string());
                    }
                }
                Err(_) => {}
            }

            resp
        })
    }
}

fn gzip_compress(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    encoder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HeaderMap, Method, StatusCode};

    #[tokio::test]
    async fn test_compression_with_gzip_accept() {
        let mw = CompressionMiddleware::new();
        let mut req = Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderMap::new(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: String::new(),
            session_data: Default::default(),
        };
        req.headers.insert("accept-encoding", "gzip, deflate");

        let body = "hello world ".repeat(100);
        let mut resp = Response::text(StatusCode::OK, &body);
        resp = mw.after(&req, resp).await;

        assert_eq!(resp.headers.get("content-encoding"), Some("gzip"));
    }

    #[tokio::test]
    async fn test_no_compression_without_accept() {
        let mw = CompressionMiddleware::new();
        let req = Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderMap::new(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: String::new(),
            session_data: Default::default(),
        };

        let body = "hello world ".repeat(100);
        let mut resp = Response::text(StatusCode::OK, &body);
        resp = mw.after(&req, resp).await;

        assert!(resp.headers.get("content-encoding").is_none());
    }

    #[tokio::test]
    async fn test_no_compression_small_body() {
        let mw = CompressionMiddleware::new();
        let mut req = Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderMap::new(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: String::new(),
            session_data: Default::default(),
        };
        req.headers.insert("accept-encoding", "gzip");

        let resp = Response::text(StatusCode::OK, "hi");
        let resp = mw.after(&req, resp).await;

        assert!(resp.headers.get("content-encoding").is_none());
    }
}
