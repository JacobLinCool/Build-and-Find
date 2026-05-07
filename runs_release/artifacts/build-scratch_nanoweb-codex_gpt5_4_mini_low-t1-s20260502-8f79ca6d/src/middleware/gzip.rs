use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;
use flate2::{write::GzEncoder, Compression};
use std::io::Write;

pub struct ResponseGzip;

#[async_trait]
impl Middleware for ResponseGzip {
    async fn after(&self, req: &Request, resp: &mut Response) {
        if req
            .header("accept-encoding")
            .map_or(false, |v| v.contains("gzip"))
            && !resp.body.is_empty()
        {
            let mut enc = GzEncoder::new(Vec::new(), Compression::default());
            let _ = enc.write_all(&resp.body);
            if let Ok(body) = enc.finish() {
                resp.body = body;
                resp.headers
                    .insert("content-encoding".into(), "gzip".into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn compresses() {
        let req = Request {
            method: crate::http::Method::Get,
            target: "/".into(),
            version: crate::http::Version::Http11,
            headers: [("accept-encoding".into(), "gzip".into())]
                .into_iter()
                .collect(),
            body: vec![],
            path_params: Default::default(),
            query_params: Default::default(),
            remote_addr: None,
            request_id: "1".into(),
        };
        let mut resp = Response::new(200, b"hello".to_vec());
        ResponseGzip.after(&req, &mut resp).await;
        assert_eq!(resp.headers.get("content-encoding").unwrap(), "gzip");
    }
}
