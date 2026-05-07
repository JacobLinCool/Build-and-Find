use crate::error::NanowebError;
use crate::http::types::{Request, Response};
use crate::middleware::{BoxFuture, Middleware, Next};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;

pub struct GzipMiddleware {
    min_size: usize,
}

impl GzipMiddleware {
    pub fn new() -> Self {
        GzipMiddleware { min_size: 1024 }
    }

    pub fn with_min_size(min_size: usize) -> Self {
        GzipMiddleware { min_size }
    }
}

impl Default for GzipMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl Middleware for GzipMiddleware {
    fn call(&self, req: Request, next: Next) -> BoxFuture<Result<Response, NanowebError>> {
        let accepts_gzip = req
            .get_header("accept-encoding")
            .map(|v| v.to_lowercase().contains("gzip"))
            .unwrap_or(false);
        let min_size = self.min_size;

        Box::pin(async move {
            let mut resp = next.run(req).await?;

            if !accepts_gzip || resp.body.len() < min_size {
                return Ok(resp);
            }

            // Don't compress if already encoded
            if resp.get_header("content-encoding").is_some() {
                return Ok(resp);
            }

            // Don't compress binary content types that are already compressed
            let ct = resp.get_header("content-type").unwrap_or("").to_lowercase();
            if ct.contains("image/")
                || ct.contains("video/")
                || ct.contains("audio/")
                || ct.contains("application/zip")
                || ct.contains("application/gzip")
            {
                return Ok(resp);
            }

            let compressed = compress_gzip(&resp.body)?;
            resp.body = compressed;
            resp.set_header("Content-Encoding", "gzip");
            resp.set_header("Content-Length", &resp.body.len().to_string());
            resp.add_header("Vary", "Accept-Encoding");

            Ok(resp)
        })
    }
}

fn compress_gzip(data: &[u8]) -> Result<Vec<u8>, NanowebError> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .map_err(|e| NanowebError::Internal(e.to_string()))?;
    encoder
        .finish()
        .map_err(|e| NanowebError::Internal(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::{HttpVersion, Method};
    use crate::middleware::{FnHandler, Next};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn make_req(accept_gzip: bool) -> Request {
        let mut headers = vec![];
        if accept_gzip {
            headers.push(("accept-encoding".to_string(), "gzip".to_string()));
        }
        Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            query: None,
            version: HttpVersion::Http11,
            headers,
            body: vec![],
            params: HashMap::new(),
            remote_addr: "127.0.0.1".into(),
            request_id: "test".into(),
        }
    }

    #[tokio::test]
    async fn test_gzip_compresses_when_accepted() {
        let big_body = "hello world ".repeat(200);
        let handler = Arc::new(FnHandler::new(move |_req: Request| {
            let b = big_body.clone();
            async move { Ok(Response::text(b)) }
        }));
        let mw: Vec<Arc<dyn Middleware>> = vec![Arc::new(GzipMiddleware::new())];
        let next = Next::new(handler, mw);
        let resp = next.run(make_req(true)).await.unwrap();
        assert_eq!(resp.get_header("content-encoding"), Some("gzip"));
    }

    #[tokio::test]
    async fn test_no_gzip_when_not_accepted() {
        let big_body = "hello world ".repeat(200);
        let handler = Arc::new(FnHandler::new(move |_req: Request| {
            let b = big_body.clone();
            async move { Ok(Response::text(b)) }
        }));
        let mw: Vec<Arc<dyn Middleware>> = vec![Arc::new(GzipMiddleware::new())];
        let next = Next::new(handler, mw);
        let resp = next.run(make_req(false)).await.unwrap();
        assert!(resp.get_header("content-encoding").is_none());
    }
}
