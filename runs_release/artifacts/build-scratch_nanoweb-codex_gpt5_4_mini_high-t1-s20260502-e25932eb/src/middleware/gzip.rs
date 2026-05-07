use std::io::Write;

use async_trait::async_trait;
use flate2::Compression;
use flate2::write::GzEncoder;

use crate::error::Result;
use crate::http::{Request, Response, StatusCode};

use super::{Middleware, Next};

pub struct GzipCompression;

#[async_trait]
impl Middleware for GzipCompression {
    async fn handle(&self, request: Request, next: Next<'_>) -> Result<Response> {
        let accepts_gzip = request
            .header("accept-encoding")
            .map(|value| value.split(',').any(|encoding| encoding.trim() == "gzip"))
            .unwrap_or(false);
        let mut response = next.run(request).await?;
        if response.status != StatusCode::NoContent
            && response.status != StatusCode::NotModified
            && !response.body.is_empty()
            && response.headers.get("content-encoding").is_none()
            && accepts_gzip
        {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder
                .write_all(&response.body)
                .map_err(crate::error::NanowebError::from)?;
            response.body = encoder.finish().map_err(crate::error::NanowebError::from)?;
            response.headers.insert("content-encoding", "gzip");
            response.headers.insert("vary", "accept-encoding");
        }
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Method, Version};
    use crate::router::Handler;
    use std::sync::Arc;

    struct TextHandler;

    #[async_trait]
    impl Handler for TextHandler {
        async fn handle(&self, _request: Request) -> Result<Response> {
            Ok(Response::text(StatusCode::Ok, "hello"))
        }
    }

    #[tokio::test]
    async fn compresses_when_requested() {
        let request = Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            query: None,
            version: Version::Http11,
            headers: {
                let mut h = crate::http::HeaderMap::new();
                h.insert("accept-encoding", "gzip");
                h
            },
            body: vec![],
            params: Default::default(),
            remote_addr: None,
            request_id: "req".into(),
            session: None,
        };
        let response = crate::middleware::execute(
            &[Arc::new(GzipCompression)],
            Arc::new(TextHandler),
            request,
        )
        .await
        .unwrap();
        assert_eq!(response.headers.get("content-encoding"), Some("gzip"));
    }
}
